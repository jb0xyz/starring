use super::action_polarity::{
    marker_is_negated, prefix_negates_action, preservation_prefix_directly_governs,
};
use super::gate_control::closed_gate_control_meaning;
use super::gate_control::{korean_safety_control_clause, marker_has_boundaries};
use super::live_scope::contains_any;
use super::unit_scope::{closed_public_secret_disclosure_subject, BoundaryKind};
use super::vocabulary::*;
use super::{
    closed_safety_control_tail, closed_without_safety_control_meaning, safety_control_action,
    word_continuation, KoreanSafetyControlClause, SafetyControlActionEffect, SafetyControlMeaning,
    ACTION_NEGATION_MODIFIERS,
};
pub(super) fn contains_hypothetical_marker(value: &str) -> bool {
    const HYPOTHETICAL_MARKERS: &[&str] = &[
        "what if",
        "what happens if",
        "suppose that",
        "assuming that",
        "hypothetically",
        "in a hypothetical",
        "would it",
        "could someone",
        "can someone",
        "is it possible",
        "explain why",
        "explain how",
        "discuss whether",
        "tell me what would",
        "만약",
        "가정하면",
        "가정해서",
        "하면 어떻게",
        "하면 무슨",
        "되는지 설명",
        "가능한지",
        "가능 여부",
        "왜 우회",
    ];
    contains_any(value, HYPOTHETICAL_MARKERS)
}

pub(super) fn contains_polite_request(value: &str) -> bool {
    const POLITE_REQUEST_MARKERS: &[&str] = &[
        "can you ",
        "could you ",
        "would you ",
        "will you ",
        "please ",
        "i want you to ",
        "i need you to ",
        "해줘",
        "해주세요",
        "해 줄래",
        "해줄래",
        "해주시",
        "부탁해",
    ];
    contains_any(value, POLITE_REQUEST_MARKERS)
}

pub(super) fn has_unnegated_boundary_action(value: &str, kind: BoundaryKind) -> bool {
    let (markers, base, third_person, passive) = match kind {
        BoundaryKind::Live => (
            LIVE_ACTIONS,
            LIVE_BASE_ACTIONS,
            LIVE_THIRD_PERSON_ACTIONS,
            LIVE_PASSIVE_ACTIONS,
        ),
        BoundaryKind::Secret => (
            SECRET_ACTIONS,
            SECRET_BASE_ACTIONS,
            SECRET_THIRD_PERSON_ACTIONS,
            SECRET_PASSIVE_ACTIONS,
        ),
        BoundaryKind::Gate => return false,
    };
    markers.iter().any(|marker| {
        value.match_indices(marker).any(|(start, matched)| {
            marker_has_boundaries(value, start, start + matched.len())
                && !marker_is_negated(value, start, start + matched.len())
                && boundary_action_is_authoritative(value, start)
                && closed_boundary_action_form(
                    value,
                    start,
                    marker,
                    kind,
                    base,
                    third_person,
                    passive,
                )
        })
    })
}

pub(super) fn boundary_action_is_authoritative(value: &str, action_start: usize) -> bool {
    let prefix = value[..action_start].trim_end();
    ![
        "simulate",
        "simulates",
        "simulated",
        "simulating",
        "simulation of",
    ]
    .iter()
    .any(|carrier| prefix.ends_with(carrier))
}

pub(super) fn closed_boundary_action_form(
    value: &str,
    start: usize,
    marker: &str,
    kind: BoundaryKind,
    base: &[&str],
    third_person: &[&str],
    passive: &[&str],
) -> bool {
    let end = start.saturating_add(marker.len());
    if !marker.is_ascii() && !closed_korean_boundary_action(value, start, end, marker, kind) {
        return false;
    }
    let typed =
        base.contains(&marker) || third_person.contains(&marker) || passive.contains(&marker);
    !typed
        || (base.contains(&marker) && closed_base_boundary_action(value, start, kind))
        || (third_person.contains(&marker)
            && closed_third_person_boundary_actor(value, start, kind))
        || (passive.contains(&marker) && closed_passive_boundary_action(value, start))
}

pub(super) fn closed_korean_boundary_action(
    value: &str,
    _start: usize,
    end: usize,
    marker: &str,
    kind: BoundaryKind,
) -> bool {
    if kind == BoundaryKind::Secret && marker == "공개" {
        let suffix = &value[end..];
        if suffix.starts_with(char::is_whitespace)
            && ["채널", "패널", "메시지", "응답", "서버"]
                .iter()
                .any(|target| suffix.trim_start().starts_with(target))
        {
            return false;
        }
    }
    true
}

pub(super) fn closed_third_person_boundary_actor(
    value: &str,
    action_start: usize,
    kind: BoundaryKind,
) -> bool {
    let prefix = value[..action_start].trim_end();
    let mut words = prefix.split_whitespace().rev();
    let mut actor = words.next();
    let mut adverbs = 0usize;
    while adverbs < 2 && actor.is_some_and(closed_boundary_action_adverb) {
        actor = words.next();
        adverbs = adverbs.saturating_add(1);
    }
    if actor == Some("that") {
        actor = words.next();
    }
    actor.is_some_and(|actor| closed_third_person_actor(kind, actor))
        || (kind == BoundaryKind::Secret
            && closed_public_secret_disclosure_subject(value, action_start))
}

pub(super) fn closed_third_person_actor(kind: BoundaryKind, actor: &str) -> bool {
    CLOSED_THIRD_PERSON_BOUNDARY_ACTORS.contains(&actor)
        || (kind == BoundaryKind::Secret && CLOSED_SECRET_DISCLOSURE_ACTORS.contains(&actor))
}

pub(in super::super) fn closed_boundary_action_adverb(word: &str) -> bool {
    CLOSED_BOUNDARY_ACTION_ADVERBS.contains(&word)
        || (word.len() > 2
            && word.ends_with("ly")
            && word.bytes().all(|byte| byte.is_ascii_alphabetic()))
}

pub(super) fn closed_base_boundary_action(
    value: &str,
    action_start: usize,
    kind: BoundaryKind,
) -> bool {
    let prefix = value[..action_start].trim_end();
    if prefix.is_empty() {
        return true;
    }
    if kind == BoundaryKind::Live && closed_first_person_live_request(prefix) {
        return true;
    }
    if [
        "carry out",
        "do this:",
        "execute",
        "execute:",
        "perform",
        "please carry out",
    ]
    .iter()
    .any(|wrapper| prefix.ends_with(wrapper))
    {
        return true;
    }
    if kind == BoundaryKind::Live
        && ["make a", "make the"]
            .iter()
            .any(|wrapper| prefix.ends_with(wrapper))
    {
        return true;
    }
    let mut words = prefix.split_whitespace().rev();
    let mut head = words.next();
    while head.is_some_and(closed_boundary_action_adverb) {
        head = words.next();
    }
    let Some(head) = head else {
        return true;
    };
    if matches!(
        head,
        "can" | "could" | "may" | "might" | "must" | "please" | "should" | "to" | "will" | "would"
    ) || closed_third_person_actor(kind, head)
    {
        return true;
    }
    if head == "do" {
        return true;
    }
    if matches!(head, "does" | "did") {
        return words
            .next()
            .is_some_and(|actor| closed_third_person_actor(kind, actor));
    }
    head == "you"
        && words.next().is_some_and(|word| {
            matches!(
                word,
                "can" | "could" | "may" | "might" | "should" | "will" | "would"
            )
        })
}

pub(super) fn closed_first_person_live_request(prefix: &str) -> bool {
    let mut words = prefix.split_whitespace().collect::<Vec<_>>();
    while words
        .last()
        .is_some_and(|word| closed_boundary_action_adverb(word))
    {
        words.pop();
    }
    matches!(words.as_slice(), ["let's" | "let’s"] | ["let", "us"])
}

pub(super) fn closed_passive_boundary_action(value: &str, action_start: usize) -> bool {
    let prefix = value[..action_start].trim_end();
    let mut words = prefix.split_whitespace().rev();
    let mut head = words.next();
    let mut adverbs = 0usize;
    while adverbs < 2 && head.is_some_and(closed_boundary_action_adverb) {
        head = words.next();
        adverbs = adverbs.saturating_add(1);
    }
    let Some(head) = head else {
        return false;
    };
    if matches!(
        head,
        "am" | "are"
            | "be"
            | "been"
            | "being"
            | "get"
            | "gets"
            | "got"
            | "gotten"
            | "is"
            | "was"
            | "were"
    ) {
        return true;
    }
    false
}

pub(in super::super) fn has_negated_gate_action_marker(value: &str) -> bool {
    closed_gate_control_meaning(value) == Some(SafetyControlMeaning::PreservesControl)
        || korean_safety_control_clause(value)
            == Some(KoreanSafetyControlClause::Control(
                SafetyControlMeaning::PreservesControl,
            ))
}

pub(super) fn without_control_meaning(value: &str) -> Option<SafetyControlMeaning> {
    let words = value.split_whitespace().collect::<Vec<_>>();
    if let Some(meaning) = closed_without_safety_control_meaning(&words) {
        return Some(meaning);
    }
    let without = words.iter().position(|word| *word == "without")?;
    without_complement_weakens_control(&words[without.saturating_add(1)..]).map(|weakens| {
        if weakens {
            SafetyControlMeaning::WeakensControl
        } else {
            SafetyControlMeaning::PreservesControl
        }
    })
}

pub(super) fn without_complement_weakens_control(words: &[&str]) -> Option<bool> {
    if closed_safety_control_tail(words) {
        return Some(true);
    }
    let action = safety_control_action(words)?;
    closed_safety_control_tail(&words[action.length..]).then_some(match action.effect {
        SafetyControlActionEffect::WeakensControl => false,
        SafetyControlActionEffect::EnforcesControl => true,
    })
}

pub(in super::super) fn has_negated_boundary_action_marker(
    value: &str,
    kind: BoundaryKind,
) -> bool {
    match kind {
        BoundaryKind::Gate => has_negated_gate_action_marker(value),
        BoundaryKind::Live => marker_sets_have_negated_action(value, &[LIVE_ACTIONS]),
        BoundaryKind::Secret => marker_sets_have_negated_action(value, &[SECRET_ACTIONS]),
    }
}

pub(super) fn marker_sets_have_negated_action(value: &str, marker_sets: &[&[&str]]) -> bool {
    marker_sets.iter().copied().flatten().any(|marker| {
        value.match_indices(marker).any(|(start, matched)| {
            marker_has_boundaries(value, start, start + matched.len())
                && marker_is_negated(value, start, start + matched.len())
        })
    })
}

pub(in super::super) fn boundary_action_is_effectively_preserved(
    value: &str,
    kind: BoundaryKind,
) -> bool {
    has_negated_boundary_action_marker(value, kind)
        || (kind == BoundaryKind::Gate
            && closed_gate_control_meaning(value) == Some(SafetyControlMeaning::PreservesControl))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreservationContinuation {
    Gerund,
    Infinitive,
}

pub(super) fn direct_preservation_continuation(
    value: &str,
    kind: BoundaryKind,
) -> Option<PreservationContinuation> {
    if kind != BoundaryKind::Gate
        || closed_gate_control_meaning(value) != Some(SafetyControlMeaning::PreservesControl)
    {
        return None;
    }
    let continuation = boundary_action_markers(kind).find_map(|marker| {
        value.match_indices(marker).find_map(|(start, matched)| {
            marker_has_boundaries(value, start, start + matched.len())
                .then(|| &value[..start])
                .filter(|prefix| {
                    preservation_prefix_directly_governs(prefix) && prefix_negates_action(prefix)
                })
                .map(|prefix| {
                    if prefix.trim_end().ends_with("refuse to") {
                        PreservationContinuation::Infinitive
                    } else {
                        PreservationContinuation::Gerund
                    }
                })
        })
    })?;
    Some(continuation)
}

pub(super) fn starts_with_preservation_action(
    value: &str,
    kind: BoundaryKind,
    continuation: PreservationContinuation,
) -> bool {
    let mut value = value;
    while let Some(stripped) = ACTION_NEGATION_MODIFIERS.iter().find_map(|modifier| {
        value.strip_prefix(modifier).and_then(|suffix| {
            suffix
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
                .then_some(suffix.trim_start())
        })
    }) {
        value = stripped;
    }
    let words = value.split_whitespace().collect::<Vec<_>>();
    if kind == BoundaryKind::Gate {
        return safety_control_action(&words).is_some_and(|action| {
            action.matches_gerund(continuation == PreservationContinuation::Gerund)
        });
    }
    boundary_action_markers(kind)
        .filter_map(|marker| marker.split_whitespace().next())
        .filter(|marker| {
            marker.ends_with("ing") == (continuation == PreservationContinuation::Gerund)
        })
        .any(|marker| {
            value.strip_prefix(marker).is_some_and(|suffix| {
                suffix
                    .chars()
                    .next()
                    .is_none_or(|character| !word_continuation(character))
            })
        })
}

pub(super) fn boundary_action_markers(
    kind: BoundaryKind,
) -> impl Iterator<Item = &'static str> + Clone {
    const GATE: &[&[&str]] = &[
        GATE_ACTIONS,
        GATE_DESTRUCTIVE_ACTIONS,
        GATE_EXACT_ACTIONS,
        GATE_REQUIREMENT_REVERSAL_ACTIONS,
    ];
    const LIVE: &[&[&str]] = &[LIVE_ACTIONS];
    const SECRET: &[&[&str]] = &[SECRET_ACTIONS];
    let marker_sets: &'static [&'static [&'static str]] = match kind {
        BoundaryKind::Gate => GATE,
        BoundaryKind::Live => LIVE,
        BoundaryKind::Secret => SECRET,
    };
    marker_sets.iter().copied().flatten().copied()
}
