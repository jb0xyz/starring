use std::collections::BTreeSet;

use super::super::super::intent_interpretation::IntentBoundaryRequestV2;
use super::super::classification::{
    boundary_action_is_effectively_preserved, closed_gate_control_weakening,
    inherited_action_negation, starts_with_secret_target_object, BoundaryKind, UnitFacts,
    SECRET_ACTIONS,
};
use super::super::syntax::{word_continuation, BoundaryUnit, TextSpan, UnitLink};
use super::coverage::{
    boundary_coverage_spans, marker_occurrence_spans, positive_role_spans_for_unit,
    unit_is_boundary_component_only,
};

#[cfg(test)]
use super::{EVIDENCE_SPAN_LINEAR_WORK, MERGED_SPAN_LOCAL_WORK};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) struct BoundaryEvidenceGroup {
    kind: BoundaryKind,
    coverage_spans: Vec<TextSpan>,
    positive_role_spans: Vec<TextSpan>,
}

impl BoundaryEvidenceGroup {
    pub(in super::super) fn request(&self) -> IntentBoundaryRequestV2 {
        self.kind.request()
    }

    pub(in super::super) fn covers_candidate(&self, candidate: TextSpan, visible: &[char]) -> bool {
        group_covers_candidate(self, candidate, visible)
    }
}

pub(in super::super) fn evidence_groups(
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

pub(in super::super) fn cross_sentence_role_bridge(
    left: &BoundaryUnit,
    right: &BoundaryUnit,
) -> bool {
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

pub(in super::super) fn coordinated_groups_cover_candidate(
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

pub(super) fn merged_spans(mut spans: Vec<TextSpan>) -> Vec<TextSpan> {
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
