use super::action_authority::closed_boundary_action_adverb;
use super::live_scope::contains_bounded_any;
use super::vocabulary::*;
use super::{
    word_continuation, ACTION_NEGATION_MODIFIERS, ACTION_POLARITY_TOKEN_WINDOW,
    ORDINARY_PREFIX_NEGATIONS, PRESERVATION_ACTOR_TERMS, PRESERVATION_DETERMINERS,
    PRESERVATION_PREFIX_NEGATIONS,
};
pub(super) fn marker_is_negated(value: &str, start: usize, end: usize) -> bool {
    let prefix = &value[..start];
    let suffix = following_chars(value, end, 32);
    prefix_negates_action(prefix) || suffix_negates_action(&suffix)
}

pub(in super::super) fn prefix_negates_action(prefix: &str) -> bool {
    action_prefix_polarity(prefix).0
}

pub(super) fn preservation_prefix_directly_governs(prefix: &str) -> bool {
    action_prefix_polarity(prefix).1
}

pub(super) fn action_prefix_polarity(prefix: &str) -> (bool, bool) {
    let mut words = prefix
        .split_whitespace()
        .rev()
        .take(ACTION_POLARITY_TOKEN_WINDOW)
        .collect::<Vec<_>>();
    words.reverse();
    action_prefix_polarity_words(&words)
}

pub(super) fn action_prefix_polarity_words(words: &[&str]) -> (bool, bool) {
    let words = &words[words.len().saturating_sub(ACTION_POLARITY_TOKEN_WINDOW)..];
    let mut end = words.len();
    let mut controls = 0usize;
    let mut closest_preservation = false;
    loop {
        if end >= 2
            && words[end.saturating_sub(2)] == "not"
            && matches!(words[end.saturating_sub(1)], "just" | "only")
        {
            end = end.saturating_sub(2);
            continue;
        }
        while end > 0
            && (ACTION_NEGATION_MODIFIERS.contains(&words[end - 1])
                || closed_boundary_action_adverb(words[end - 1])
                || matches!(
                    words[end - 1],
                    "be" | "been" | "being" | "get" | "gets" | "got" | "gotten"
                ))
        {
            end = end.saturating_sub(1);
        }
        let matched = trailing_preservation_object_frame(words, end)
            .map(|start| (start, true))
            .or_else(|| trailing_passive_preservation_frame(words, end).map(|start| (start, true)))
            .or_else(|| trailing_negative_allow_frame(words, end).map(|start| (start, false)))
            .or_else(|| trailing_negative_actor_modal(words, end).map(|start| (start, false)))
            .or_else(|| {
                trailing_control(words, end, PRESERVATION_PREFIX_NEGATIONS)
                    .map(|start| (start, true))
            })
            .or_else(|| {
                trailing_control(words, end, ORDINARY_PREFIX_NEGATIONS).map(|start| (start, false))
            });
        let Some((start, preservation)) = matched else {
            break;
        };
        if controls == 0 {
            closest_preservation = preservation;
        }
        controls = controls.saturating_add(1);
        end = start;
    }
    (controls % 2 == 1, closest_preservation)
}

pub(super) fn trailing_passive_preservation_frame(words: &[&str], end: usize) -> Option<usize> {
    let predicate = end.checked_sub(2)?;
    (words[end.saturating_sub(1)] == "from"
        && matches!(
            words[predicate],
            "blocked" | "disallowed" | "forbidden" | "prevented" | "prohibited" | "stopped"
        )
        && closed_passive_actor_start(words, predicate).is_some())
    .then_some(predicate)
}

pub(super) fn trailing_negative_allow_frame(words: &[&str], end: usize) -> Option<usize> {
    if end < 3 || words[end.saturating_sub(1)] != "to" {
        return None;
    }
    let allow = (0..end.saturating_sub(1)).rev().find(|index| {
        matches!(
            words[*index],
            "allow" | "allowed" | "allowing" | "allows" | "permit" | "permits" | "permitting"
        )
    })?;
    let object = &words[allow.saturating_add(1)..end.saturating_sub(1)];
    let actor_is_closed = if object.is_empty() {
        words[allow] == "allowed" && closed_passive_actor_start(words, allow).is_some()
    } else {
        closed_actor_terms(object)
    };
    if !actor_is_closed {
        return None;
    }
    action_prefix_polarity_words(&words[..allow]).0.then_some(0)
}

pub(super) fn closed_passive_actor_start(words: &[&str], predicate: usize) -> Option<usize> {
    let mut auxiliary_end = predicate;
    while auxiliary_end > 0
        && (ACTION_NEGATION_MODIFIERS.contains(&words[auxiliary_end - 1])
            || ORDINARY_PREFIX_NEGATIONS
                .iter()
                .flat_map(|control| control.split_whitespace())
                .any(|term| term == words[auxiliary_end - 1]))
    {
        auxiliary_end = auxiliary_end.saturating_sub(1);
    }
    let auxiliary = auxiliary_end.checked_sub(1)?;
    if !matches!(
        words[auxiliary],
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
        return None;
    }
    (1..=3).rev().find_map(|length| {
        let start = auxiliary.checked_sub(length)?;
        closed_actor_terms(&words[start..auxiliary]).then_some(start)
    })
}

pub(super) fn closed_actor_terms(words: &[&str]) -> bool {
    let words = if words
        .first()
        .is_some_and(|word| PRESERVATION_DETERMINERS.contains(word))
    {
        &words[1..]
    } else {
        words
    };
    !words.is_empty()
        && words.len() <= 2
        && words
            .iter()
            .all(|word| closed_preservation_actor_term(word))
}

pub(super) fn closed_preservation_actor_term(word: &str) -> bool {
    PRESERVATION_ACTOR_TERMS.contains(&word) || CLOSED_THIRD_PERSON_BOUNDARY_ACTORS.contains(&word)
}

pub(super) fn trailing_negative_actor_modal(words: &[&str], end: usize) -> Option<usize> {
    let modal = end.checked_sub(1)?;
    if !matches!(
        words[modal],
        "can" | "could" | "may" | "might" | "must" | "should" | "will" | "would"
    ) {
        return None;
    }
    if modal >= 1 && matches!(words[modal - 1], "nobody" | "none") {
        return Some(modal - 1);
    }
    if modal >= 2 && words[modal - 2] == "no" && words[modal - 1] == "one" {
        return Some(modal - 2);
    }
    if modal >= 2 && words[modal - 2] == "no" && closed_preservation_actor_term(words[modal - 1]) {
        Some(modal - 2)
    } else {
        None
    }
}

pub(super) fn trailing_control(words: &[&str], end: usize, controls: &[&str]) -> Option<usize> {
    controls.iter().find_map(|control| {
        let length = control.split_whitespace().count();
        let start = end.checked_sub(length)?;
        words[start..end]
            .iter()
            .copied()
            .eq(control.split_whitespace())
            .then_some(start)
    })
}

pub(super) fn trailing_preservation_object_frame(words: &[&str], end: usize) -> Option<usize> {
    if end < 3 || words[end - 1] != "from" {
        return None;
    }
    let object_end = end.saturating_sub(1);
    for object_length in 1..=3 {
        let Some(object_start) = object_end.checked_sub(object_length) else {
            continue;
        };
        let objects = &words[object_start..object_end];
        let objects = if objects
            .first()
            .is_some_and(|word| PRESERVATION_DETERMINERS.contains(word))
        {
            &objects[1..]
        } else {
            objects
        };
        if objects.is_empty()
            || objects.len() > 2
            || (!objects
                .iter()
                .all(|word| closed_preservation_actor_term(word))
                && !closed_boundary_preservation_object(objects))
        {
            continue;
        }
        if let Some(start) = trailing_control(words, object_start, PRESERVATION_PREFIX_NEGATIONS) {
            return Some(start);
        }
    }
    None
}

pub(super) fn closed_boundary_preservation_object(words: &[&str]) -> bool {
    let value = words.join(" ");
    contains_bounded_any(&value, SECRET_TARGETS)
        || matches!(value.as_str(), "live changes" | "production changes")
}

pub(in super::super) fn suffix_negates_action(suffix: &str) -> bool {
    let suffix = suffix.trim_start();
    if korean_suffix_negates_action(suffix).is_some_and(|negated| negated) {
        return true;
    }
    SUFFIX_NEGATIONS.iter().any(|negation| {
        suffix.strip_prefix(negation).is_some_and(|remaining| {
            remaining
                .chars()
                .next()
                .is_none_or(|character| !word_continuation(character))
        })
    })
}

pub(super) fn korean_suffix_negates_action(value: &str) -> Option<bool> {
    let remainder = [
        "되지 않게",
        "되지 못하게",
        "지 않게",
        "지 못하게",
        "를 막",
        "을 막",
        "를 금지",
        "을 금지",
    ]
    .iter()
    .find_map(|marker| value.strip_prefix(marker))?;
    let preservation_negated = ["지 말", "지마", "하지 마", "하지마"]
        .iter()
        .any(|marker| remainder.contains(marker));
    Some(!preservation_negated)
}

pub(super) fn following_chars(value: &str, start: usize, limit: usize) -> String {
    value[start..].chars().take(limit).collect()
}
