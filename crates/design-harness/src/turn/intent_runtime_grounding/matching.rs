#[cfg(test)]
use std::cell::Cell;

use super::patterns::*;

#[cfg(test)]
thread_local! {
    static TERM_OCCURRENCE_SCANS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn has_any(text: &str, candidates: &[&str]) -> bool {
    candidates.iter().any(|candidate| has_term(text, candidate))
}

pub(super) fn requirement_action_owns(
    text: &str,
    target_subjects: &[&str],
    maximum_words: usize,
) -> bool {
    let all_subjects = PERSISTENT_STATE_MARKERS
        .iter()
        .chain(DURABLE_TIMER_MARKERS)
        .chain(ECONOMY_PERSISTENCE_MARKERS)
        .copied()
        .collect::<Vec<_>>();
    let next_target = next_term_start(text, target_subjects);
    let next_subject = next_term_start(text, &all_subjects);
    let missing = text.len().saturating_add(1);
    POSITIVE_REQUIREMENT_ACTIONS
        .iter()
        .flat_map(|action| term_occurrences(text, action))
        .any(|(_, action_end)| {
            let target = next_target[action_end];
            let nearest = next_subject[action_end];
            target != missing
                && target == nearest
                && target.saturating_sub(action_end) <= MAXIMUM_PROXIMITY_BYTES
                && text[action_end..target].split_whitespace().count() <= maximum_words
        })
}

pub(super) fn ordered_near(
    text: &str,
    subjects: &[&str],
    actions: &[&str],
    maximum_words: usize,
) -> bool {
    let next_action = next_term_start(text, actions);
    let missing = text.len().saturating_add(1);
    subjects.iter().any(|subject| {
        term_occurrences(text, subject).any(|(_, subject_end)| {
            let action_start = next_action[subject_end];
            action_start != missing
                && action_start.saturating_sub(subject_end) <= MAXIMUM_PROXIMITY_BYTES
                && text[subject_end..action_start].split_whitespace().count() <= maximum_words
        })
    })
}

pub(super) fn llm_before_action_near(text: &str, actions: &[&str], maximum_words: usize) -> bool {
    let next_action = next_term_start(text, actions);
    let missing = text.len().saturating_add(1);
    runtime_llm_occurrences(text)
        .into_iter()
        .any(|(_, subject_end)| {
            let action_start = next_action[subject_end];
            action_start != missing
                && action_start.saturating_sub(subject_end) <= MAXIMUM_PROXIMITY_BYTES
                && text[subject_end..action_start].split_whitespace().count() <= maximum_words
        })
}

pub(super) fn llm_before_setup_execution(text: &str, maximum_words: usize) -> bool {
    let missing = text.len().saturating_add(1);
    let next_setup = next_occurrence_start(
        text.len(),
        SETUP_TIME_SUFFIXES
            .iter()
            .flat_map(|marker| term_occurrences(text, marker))
            .map(|(start, _)| start)
            .filter(|start| !setup_marker_qualifies_artifact(text, *start)),
    );
    runtime_llm_occurrences(text)
        .into_iter()
        .any(|(_, llm_end)| {
            let setup_start = next_setup[llm_end];
            setup_start != missing
                && setup_start.saturating_sub(llm_end) <= MAXIMUM_PROXIMITY_BYTES
                && text[llm_end..setup_start].split_whitespace().count() <= maximum_words
        })
}

fn setup_marker_qualifies_artifact(text: &str, setup_start: usize) -> bool {
    let mut prefix = text[..setup_start].trim_end();
    while let Some(head) = ["earlier", "only", "once", "specifically"]
        .iter()
        .find_map(|adverb| prefix.strip_suffix(adverb))
    {
        if !head.chars().next_back().is_some_and(char::is_whitespace) {
            break;
        }
        prefix = head.trim_end();
    }
    SETUP_ARTIFACT_PASSIVES
        .iter()
        .any(|passive| term_occurrences(prefix, passive).any(|(_, end)| end == prefix.len()))
}

pub(super) fn action_before_llm_near(text: &str, actions: &[&str], maximum_words: usize) -> bool {
    let missing = text.len().saturating_add(1);
    let next_llm = next_occurrence_start(
        text.len(),
        runtime_llm_occurrences(text)
            .into_iter()
            .map(|(start, _)| start),
    );
    actions.iter().any(|action| {
        term_occurrences(text, action).any(|(_, action_end)| {
            let llm_start = next_llm[action_end];
            llm_start != missing
                && llm_start.saturating_sub(action_end) <= MAXIMUM_PROXIMITY_BYTES
                && text[action_end..llm_start].split_whitespace().count() <= maximum_words
        })
    })
}

pub(super) fn has_runtime_llm_marker(text: &str) -> bool {
    !runtime_llm_occurrences(text).is_empty()
}

fn runtime_llm_occurrences(text: &str) -> Vec<(usize, usize)> {
    LLM_MARKERS
        .iter()
        .flat_map(|marker| term_occurrences(text, marker))
        .filter(|(_, end)| {
            !NON_MODEL_LLM_SURFACES
                .iter()
                .any(|surface| text[*end..].starts_with(surface))
        })
        .collect()
}

fn next_term_start(text: &str, candidates: &[&str]) -> Vec<usize> {
    next_occurrence_start(
        text.len(),
        candidates
            .iter()
            .flat_map(|candidate| term_occurrences(text, candidate))
            .map(|(start, _)| start),
    )
}

fn next_occurrence_start(text_len: usize, starts: impl Iterator<Item = usize>) -> Vec<usize> {
    let missing = text_len.saturating_add(1);
    let mut next = vec![missing; text_len.saturating_add(1)];
    for start in starts {
        next[start] = start;
    }
    let mut nearest = missing;
    for index in (0..next.len()).rev() {
        if next[index] != missing {
            nearest = next[index];
        }
        next[index] = nearest;
    }
    next
}

#[cfg(test)]
pub(super) fn requirement_action_occurrence_scans(text: &str) -> usize {
    TERM_OCCURRENCE_SCANS.with(|scans| scans.set(0));
    let _ = requirement_action_owns(text, DURABLE_TIMER_MARKERS, 8);
    TERM_OCCURRENCE_SCANS.with(Cell::get)
}

fn term_occurrences<'a>(
    text: &'a str,
    candidate: &'a str,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    #[cfg(test)]
    TERM_OCCURRENCE_SCANS.with(|scans| scans.set(scans.get().saturating_add(1)));
    text.match_indices(candidate).filter_map(move |(start, _)| {
        let end = start.saturating_add(candidate.len());
        let left = text[..start].chars().next_back();
        let right = text[end..].chars().next();
        let bounded = candidate.starts_with(' ')
            || candidate.ends_with(' ')
            || !candidate.is_ascii()
            || (!left.is_some_and(ascii_word_character)
                && !right.is_some_and(ascii_word_character));
        bounded.then_some((start, end))
    })
}

fn has_term(text: &str, candidate: &str) -> bool {
    if candidate.starts_with(' ') || candidate.ends_with(' ') || !candidate.is_ascii() {
        return text.contains(candidate);
    }
    text.match_indices(candidate).any(|(start, _)| {
        let end = start.saturating_add(candidate.len());
        let left = text[..start].chars().next_back();
        let right = text[end..].chars().next();
        !left.is_some_and(ascii_word_character) && !right.is_some_and(ascii_word_character)
    })
}

fn ascii_word_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}
